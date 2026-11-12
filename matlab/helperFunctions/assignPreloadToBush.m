function bushStruct = assignPreloadToBush(currentStep,bushStruct,bushPreloadStruct)
    % assignPreloadToBush Updates the preload values in bush structures for a given step.
    %
    % This function iterates through each bush element in the provided bushStruct 
    % and updates its preload value based on the current simulation step. The function 
    % checks if each bush element is active and, if so, assigns the corresponding 
    % preload value from the bushPreloadStruct for the current step. This operation is 
    % crucial for simulating varying preload conditions in mechanical systems over time.
    %
    % Inputs:
    %   currentStep      - The current time step in the simulation process.
    %   bushStruct       - An array of structs representing bush elements, each with 
    %                      properties including whether the bush is active and its current preload.
    %   bushPreloadStruct - An array of structs parallel to bushStruct, containing 
    %                       the preload values for each bush element at different steps.
    %
    % Output:
    %   bushStruct       - The updated array of bush structs with the preload values 
    %                      for the current step assigned to active bushes.
    %
    for i = 1:length(bushStruct)
        if bushStruct(i).active && bushStruct(i).isPreload
            bushStruct(i).preloadCurrent = bushPreloadStruct(i).preloadCurrent(:,currentStep);
        end
    end
end
